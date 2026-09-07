"""Conservative Makefile target-ownership checks, without executing recipes."""
import re
from .review import finding

def destructive_target_overlap(path,text):
    lines=text.splitlines();variables={};rules={}
    for line in lines:
        match=re.match(r'^([A-Za-z_]\w*)\s*(?::|\?)?=\s*(.*?)\s*$',line)
        if match:variables[match[1]]=match[2]
    def expand(value):
        for _ in range(8):
            new=re.sub(r'\$\(([A-Za-z_]\w*)\)',lambda m:variables.get(m[1],m[0]),value)
            if new==value:break
            value=new
        return value
    current=None
    for index,line in enumerate(lines):
        if line.startswith('\t'):
            if current:rules[current]['recipes'].append(line)
            continue
        match=re.match(r'^([^\s#:=]+)\s*:(?!=)(.*)$',line)
        if not match:continue
        target=expand(match[1]);deps=expand(match[2]).split('#',1)[0].replace('|',' ').split()
        current=None
        if any(c in target for c in '$%') or any('$' in d or '\\' in d for d in deps):continue
        current=target;rules[target]={'deps':deps,'recipes':[],'line':index+1,'header':line}
    def depends(target,ancestor,visited=None):
        visited=set() if visited is None else visited
        if target in visited:return False
        visited.add(target)
        deps=rules.get(target,{}).get('deps',[])
        return ancestor in deps or any(depends(d,ancestor,visited) for d in deps)
    result=[]
    for parent,rule in rules.items():
        if '/' not in parent:continue
        destructive=next((line for line in rule['recipes'] if re.search(r'\brm\s+-[rfRF]+\s+\$@(?:\s|$)',line)),None)
        if not destructive:continue
        for child,child_rule in rules.items():
            if not child.startswith(parent.rstrip('/')+'/') or depends(child,parent) or depends(parent,child):continue
            claim=f'Parallel Make targets {parent} and {child} have no visible ordering dependency; the parent recipe removes $@ while the child writes below it, so concurrent execution can delete the child output.'
            item=finding('build_graph','warning',claim,path,rule['line'],rule['header'],'unordered_destructive_target_overlap_'+child)
            item.update(causal_claim=claim,failure_condition='Both targets are selected for a parallel build. Includes, implicit rules and external orchestration have not been evaluated.',affected_behavior='Generated child files may disappear or dependent compilation may fail.',supporting_evidence=[{'path':path,'evidence':destructive},{'path':path,'evidence':child_rule['header']}])
            result.append(item)
            if len(result)>=4:return result
    return result
