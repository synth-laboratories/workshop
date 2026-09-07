"""Minimal source-declared QA prerequisites, never arbitrary setup execution."""
import re
import shlex

ALLOW={'build-essential':('gcc','g++','make'),'gcc':('gcc',),'g++':('g++',),'make':('make',),'git':('git',)}

def plan(files, objective):
    needs_build=bool(re.search(r'build_ext|extension|compil|build.hook|packaging.frontend',objective,re.I))
    needs_git=bool(re.search(r'git\s+(?:clone|ls-remote)',objective))
    if not (needs_build or needs_git):return None
    packages={};evidence=[]
    for path,text in files.items():
        if not path.startswith('solution/'):continue
        code='\n'.join(line for line in text.splitlines() if not line.lstrip().startswith('#')).replace('\\\n',' ')
        for match in re.finditer(r'\bapt(?:-get)?\s+install\b[^\n]*',code):
            invocation=match[0].split('&&')[0]
            for word in re.findall(r'(?<![\w-])(build-essential|gcc|g\+\+|make|git)(=[A-Za-z0-9.+:~_-]+)?(?=\s|\\|$)',invocation):
                name,version=word
                if name=='git' and not needs_git:continue
                if name!='git' and not needs_build:continue
                value=name+version
                if name in packages and packages[name]!=value:return None
                packages[name]=value
                evidence.append({'path':path,'package':value,'notice':'Only this allowlisted literal source package is eligible; arbitrary source setup is never executed.'})
    if not packages:return None
    binaries=sorted({b for p in packages for b in ALLOW[p]})
    checks='; '.join('command -v '+shlex.quote(b)+' >/dev/null 2>&1 || qa_need_tools=1' for b in binaries)
    package_args=' '.join(shlex.quote(v) for v in packages.values())
    command=('qa_need_tools=0; '+checks+'; '
             'if [ "$qa_need_tools" = 1 ]; then '
             'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends '+package_args+' || exit $?; fi; '
             'printf "QA_SOURCE_TOOLCHAIN_READY\\n"; '+
             '; '.join('command -v '+shlex.quote(b) for b in binaries)+'; '
             "dpkg-query -W -f='${Package} ${Version}\\n' "+' '.join(shlex.quote(p) for p in packages))
    return {'command':command,'packages':list(packages.values()),'source_evidence':evidence,
            'notice':'QA prerequisite instrumentation, not a task solve or successful compatibility check. Exact literal source package pins are preserved for installation. Existing tools are version-recorded, not assumed version-equivalent. No package-pin fallback or host installation.'}
