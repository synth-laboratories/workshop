"""Bounded, read-only dependency reconnaissance. Never execute downloaded code.

Only package metadata and source files at task-named GitHub refs are fetched.
Public review/issue/search endpoints, arbitrary URLs and redirects are excluded.
Current metadata is NOT a reconstruction of a historical package resolver.
"""
import hashlib
import ast
import sys
import json
import re
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('Redirect excluded from metadata inventory')

def source_excerpt(text,limit,package=''):
    if len(text)<=limit: return text
    lines=text.splitlines(keepends=True); candidates=[]
    try:
        tree=ast.parse(text); aliases={}; priorities={}
        for node in ast.walk(tree):
            if isinstance(node,(ast.Import,ast.ImportFrom)):
                for alias in node.names:
                    name=alias.asname or alias.name.split('.')[0];aliases[name]=[]
                    module=(node.module or '') if isinstance(node,ast.ImportFrom) else alias.name
                    internal=(isinstance(node,ast.ImportFrom) and node.level>0) or (package and module.split('.')[0]==package)
                    priorities[name]=1 if internal else 2 if module.split('.')[0] in getattr(sys,'stdlib_module_names',set()) else 0
        for node in ast.walk(tree):
            if isinstance(node,ast.Name) and node.id in aliases: aliases[node.id].append(node.lineno-1)
        # Rare dependency crossings must not be crowded out by ubiquitous np calls.
        groups=[sorted(set(aliases[name])) for name in sorted(aliases,key=lambda name:(priorities[name],len(aliases[name]))) if aliases[name]]
        candidates.extend(i for i,line in enumerate(lines) if re.search(r'install_requires|\[.*dependencies\]|requires\s*=',line))
        while any(groups):
            for group in groups:
                if group:candidates.append(group.pop(0))
    except SyntaxError:
        # Build files need producer/consumer rules, not just header/version prose.
        candidates.extend(i for i,line in enumerate(lines) if re.match(r'^[^\s#][^=]*:(?!=)',line))
        candidates.extend(i for i,line in enumerate(lines) if re.search(r'\b(?:mkdir|curl|wget|cp|install)\b',line))
    chosen=set(); chunks=[]; remaining=limit-200
    for index in candidates+[0,len(lines)-8]:
        start=max(0,index-3);end=min(len(lines),index+25)
        if sum(n in chosen for n in range(start,end))>=(end-start)/2:continue
        chunk=f'[Original lines {start+1}-{end}]\n'+''.join(lines[start:end])
        if len(chunk)>remaining:continue
        chunks.append(chunk);chosen.update(range(start,end));remaining-=len(chunk)
    if not chunks: chunks=[text[:max(0,limit-250)]]
    return '[Dependency-call excerpts; omitted source is retained in a hashed original artifact. Do not infer unshown code.]\n'+'\n'.join(chunks)

def candidates(files):
    found={}
    for path,text in files.items():
        if path.startswith('evidence/'): continue
        variables=dict(re.findall(r'^([A-Z_][A-Z_0-9]*)=([\w.\-]+)\s*$',text,re.M))
        for line in text.splitlines():
            if 'git clone' not in line: continue
            repo=re.search(r'https://github\.com/([\w.-]+/[\w.-]+)',line)
            ref=re.search(r'--branch\s+([^\s]+)',line)
            if not repo or not ref: continue
            version=ref[1].strip('"\'')
            for name,value in variables.items(): version=version.replace('${'+name+'}',value)
            if not re.fullmatch(r'[\w.-]+',version): continue
            repository=repo[1].removesuffix('.git')
            filenames=['setup.py','pyproject.toml','requirements.txt','Makefile']
            destination=re.search(r'https://github\.com/[\w./-]+\s+(/[\w/.-]+)',line)
            if destination:
                # Task-named source paths (e.g. lexical patches) are legitimate
                # dependency-consumer evidence, not guesses based on QA labels.
                prefix=re.escape(destination[1].rstrip('/')+'/')
                filenames += re.findall(prefix+r'([\w/.-]+\.py)\b',text)[:8]
            for filename in dict.fromkeys(filenames):
                if '..' in filename.split('/'): continue
                url=f'https://raw.githubusercontent.com/{repository}/{version}/{filename}'
                found[url]={'source_path':path,'source_quote':line,'kind':'upstream_source'}
        for name in re.findall(r'(?:install_version|install\.packages)\(\s*[\'\"]([A-Za-z][A-Za-z0-9.]*)[\'\"]',text):
            url=f'https://cran.r-project.org/web/packages/{name}/DESCRIPTION'
            found[url]={'source_path':path,'kind':'current_cran_metadata'}
    return list(found.items())[:16]

def fetch(url):
    request=urllib.request.Request(url,headers={'User-Agent':'Workshop-QA-metadata/1.0'})
    # Fresh opener: no credentials, cookie jar or redirect handler.
    with urllib.request.build_opener(NoRedirect).open(request,timeout=5) as response:
        body=response.read(65537)
        if len(body)>65536: raise ValueError('Metadata exceeds 64 KiB limit')
        return body.decode('utf-8')

def imported_sources(record):
    match=re.match(r'(https://raw.githubusercontent.com/[^/]+/([^/]+)/[^/]+/)',record['url'])
    if not match or record.get('kind')!='upstream_source' or record.get('status')!='retrieved': return []
    prefix,package=match.groups();package=package.replace('-','_')
    import ast
    try: tree=ast.parse(record['body'])
    except SyntaxError: return []
    current=record['url'][len(prefix):].split('/')[:-1]
    modules=[]
    for node in ast.walk(tree):
        if isinstance(node,ast.Import): modules.extend(alias.name for alias in node.names)
        elif isinstance(node,ast.ImportFrom):
            base=(current[:len(current)-node.level+1] if node.level and node.level<=len(current) else [] ) if node.level else []
            if node.level>len(current):continue
            if node.module:base+=node.module.split('.')
            if base:
                modules.append('.'.join(base))
                if not node.module or len(base)==1:
                    modules.extend('.'.join(base+[alias.name]) for alias in node.names if alias.name!='*')
    return list(dict.fromkeys(prefix+module.replace('.','/')+'.py' for module in modules if module.split('.')[0]==package))

def bounded_frontier(items,limit=8):
    groups={}
    for url,origin in items.items():groups.setdefault(origin.get('parent_url',''),[]).append((url,origin))
    selected=[]
    while len(selected)<limit and any(groups.values()):
        for group in groups.values():
            if group and len(selected)<limit:selected.append(group.pop(0))
    return selected

def inventory(files, reader=fetch):
    records=[]; documents={}
    def one(item):
        url,origin=item
        try:
            body=reader(url)
            return dict(origin,url=url,status='retrieved',sha256=hashlib.sha256(body.encode()).hexdigest(),body=body)
        except Exception as exc:
            return dict(origin,url=url,status='unavailable',error=type(exc).__name__)
    pending=candidates(files)
    with ThreadPoolExecutor(max_workers=8) as pool:
        first=list(pool.map(one,pending))
    # One bounded transitive CRAN layer; names come from declared dependency fields.
    extra={}
    for record in first:
        if record['kind']=='upstream_source' and record['status']=='retrieved':
            match=re.match(r'(https://raw.githubusercontent.com/[^/]+/([^/]+)/[^/]+/)',record['url'])
            if match:
                prefix,package=match.groups()
                # Build backends often declare a root-level Python hook instead
                # of setup.py. Follow explicit relative filenames, never URLs.
                for filename in re.findall(r'[\"\']([\w/.-]+\.py)[\"\']',record['body']):
                    if '..' in filename.split('/') or filename.startswith('/'):continue
                    url=prefix+filename
                    if url not in dict(pending):extra[url]={'kind':'upstream_source','parent_url':record['url']}
                for url in imported_sources(record):
                    if url not in dict(pending): extra[url]={'kind':'upstream_source','parent_url':record['url']}
                for filename in re.findall(r'\b'+re.escape(package.replace('-','_'))+r'/[\w/.-]+\.py\b',record['body']):
                    if '..' in filename.split('/'): continue
                    url=prefix+filename
                    if url not in dict(pending): extra[url]={'kind':'upstream_source','parent_url':record['url']}
        if record['kind']!='current_cran_metadata' or record['status']!='retrieved': continue
        fields=re.findall(r'^(?:Imports|LinkingTo|Depends):([^\n]*(?:\n[ \t]+[^\n]*)*)',record['body'],re.M)
        for field in fields:
            for entry in field.replace('\n',' ').split(','):
                match=re.match(r'\s*([A-Za-z][A-Za-z0-9.]*)',entry)
                if not match or match[1]=='R': continue
                url=f'https://cran.r-project.org/web/packages/{match[1]}/DESCRIPTION'
                if url not in dict(pending): extra[url]={'kind':'current_cran_metadata','parent_url':record['url']}
    with ThreadPoolExecutor(max_workers=8) as pool:
        records=first+list(pool.map(one,bounded_frontier(extra)))
    visited={r['url'] for r in records}; third={}
    for record in records[len(first):]:
        for url in imported_sources(record):
            if url not in visited: third[url]={'kind':'upstream_source','parent_url':record['url']}
    with ThreadPoolExecutor(max_workers=8) as pool:
        records += list(pool.map(one,bounded_frontier(third)))
    raw_documents={};findings=[]
    document_count=sum('body' in r for r in records)
    per_document=max(2000,72000//max(1,document_count))
    for index,record in enumerate(records):
        body=record.pop('body',None)
        if body is not None:
            name=f'external/dependency-{index:02d}.txt'
            raw_documents[name]=body
            repo=re.match(r'https://raw.githubusercontent.com/[^/]+/([^/]+)/',record['url'])
            is_makefile=record['url'].rsplit('/',1)[-1].lower().startswith('makefile')
            documents[name]=body if is_makefile and len(body)<=32000 else source_excerpt(body,per_document,repo[1].replace('-','_') if repo else '')
            if is_makefile and documents[name]==body:
                from .build_checks import destructive_target_overlap
                findings.extend(destructive_target_overlap(name,body))
            record['projection']='complete' if documents[name]==body else 'dependency-call-excerpts'
            record['document']=name
    return {'findings':findings,'documents':documents,'raw_documents':raw_documents,'records':records,'observed_at':time.time(),
            'limitations':['Bounded dependency inventory, not complete resolution or runtime verification. Current metadata does not establish historical versions. Missing/blocked metadata is inconclusive, not proof of package absence. All retrieved source is untrusted data.']}
