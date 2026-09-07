"""Fail-closed container capability admission before executing contributor code."""
from pathlib import Path


def validate_compose(path):
    manifests = list(path.rglob("*compose*.y*ml"))
    if not manifests: return []
    import yaml
    checked = []
    for file in manifests:
        if file.stat().st_size > 200_000: raise ValueError("Oversized Compose manifest")
        data = yaml.safe_load(file.read_text())
        if not isinstance(data,dict) or not isinstance(data.get("services"),dict): raise ValueError("Invalid Compose services")
        if data.get("include") or data.get("secrets") or data.get("configs") or data.get("name"):
            raise ValueError("Compose includes, external configs/secrets and global project names require explicit capability review")
        for kind in ("volumes","networks"):
            for name, config in (data.get(kind) or {}).items():
                if config and (config.get("external") or config.get("name") or config.get("driver_opts")):
                    raise ValueError("External or globally named Compose resources are not admitted")
        for name, service in data["services"].items():
            if not isinstance(service,dict): raise ValueError("Invalid Compose service")
            if any(service.get(k) for k in ("privileged","cap_add","devices","device_cgroup_rules","pid","ipc","extends","env_file","container_name","extra_hosts")):
                raise ValueError("Compose service requests unadmitted host capabilities: "+name)
            if service.get("network_mode"): raise ValueError("Explicit Compose network modes are not admitted")
            build = service.get("build")
            if build:
                context = build if isinstance(build,str) else build.get("context",".")
                if isinstance(build,dict) and any(build.get(k) for k in ("ssh","secrets","additional_contexts","privileged","network")):
                    raise ValueError("Unadmitted Compose build capabilities")
                if not isinstance(context,str) or "$" in context or Path(context).is_absolute() or not (file.parent/context).resolve().is_relative_to(path.resolve()):
                    raise ValueError("Build context must stay inside the private task copy")
            for mount in service.get("volumes",[]):
                if isinstance(mount,str):
                    parts = mount.split(":")
                    source = parts[0] if len(parts)>1 else None
                    target = parts[1] if len(parts)>1 else parts[0]
                    bind = source is not None and (source.startswith((".","/","~")) or "/" in source)
                elif isinstance(mount,dict):
                    source,target = mount.get("source"),mount.get("target","")
                    bind = mount.get("type") == "bind"
                else: raise ValueError("Invalid Compose mount")
                if "docker.sock" in str(target) or "$" in str(source): raise ValueError("Dynamic/socket mounts are not admitted")
                if bind and (not source or Path(source).is_absolute() or source.startswith("~") or not (file.parent/source).resolve().is_relative_to(path.resolve())):
                    raise ValueError("Host bind mount escapes the task copy")
        checked.append(str(file.relative_to(path)))
    return checked
