"""Read-only checks of image-staged paths, separate from injected QA sources."""
import posixpath
import re
import shlex

def copied_file_paths(bundle):
    environment=bundle/'environment';docker=environment/'Dockerfile';paths=[];workdir='/'
    if not docker.exists():return []
    for line in docker.read_text().splitlines():
        try:parts=shlex.split(line)
        except ValueError:continue
        if len(parts)==2 and parts[0]=='WORKDIR' and parts[1].startswith('/'):workdir=parts[1]
        if len(parts)!=3 or parts[0]!='COPY' or any(x in line for x in ('[','$','--')):continue
        source=environment/parts[1];target=parts[2]
        if not source.resolve().is_relative_to(environment.resolve()):continue
        if not target.startswith('/'):target=posixpath.join(workdir,target)
        if source.is_dir():
            candidates=[posixpath.join(target,str(f.relative_to(source))) for f in sorted(source.rglob('*')) if f.is_file()]
        elif source.is_file():
            candidates=[posixpath.join(target,source.name) if parts[2].endswith('/') or parts[2]=='.' else target]
        else:continue
        paths += [posixpath.normpath(p) for p in candidates if re.fullmatch(r'/[\w./-]+',p)]
    return list(dict.fromkeys(paths))[:32]

def visibility_command(paths):
    commands=['id']
    for path in paths:
        quoted=shlex.quote(path)
        commands.append(f"if [ -f {quoted} ] && [ -r {quoted} ]; then printf '%s\\n' {shlex.quote('READABLE '+path)}; else printf '%s\\n' {shlex.quote('NOT_READABLE '+path)}; fi")
    return '; '.join(commands)
