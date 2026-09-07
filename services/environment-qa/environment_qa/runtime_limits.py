"""Read kernel-enforced limits without allocating memory or changing cgroups."""

COMMAND = '''for qa_limit_file in /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory.swap.max /sys/fs/cgroup/cpu.max /sys/fs/cgroup/pids.max /sys/fs/cgroup/memory/memory.limit_in_bytes /sys/fs/cgroup/memory/memory.memsw.limit_in_bytes; do
if [ -r "$qa_limit_file" ]; then printf '%s=' "$qa_limit_file"; head -c 80 "$qa_limit_file"; printf '\\n'; fi
done'''


def parse(stdout):
    values={}
    for line in stdout.splitlines():
        if '=' not in line:continue
        path,value=line.split('=',1)
        if path.startswith('/sys/fs/cgroup/'):
            values[path]=value.strip()
    def number(path):
        value=values.get(path,'')
        return int(value) if value.isdigit() else None
    memory=number('/sys/fs/cgroup/memory.max')
    swap=number('/sys/fs/cgroup/memory.swap.max')
    if memory is None and '/sys/fs/cgroup/memory.max' not in values:
        memory=number('/sys/fs/cgroup/memory/memory.limit_in_bytes')
        combined=number('/sys/fs/cgroup/memory/memory.memsw.limit_in_bytes')
        if memory is not None and combined is not None and combined>=memory:swap=combined-memory
    return {'raw':values,'memory_limit_bytes':memory,'swap_limit_bytes':swap,
            'notice':'Observed configuration only. Null means unavailable or unlimited, not zero. Swap allowance is not evidence of actual swap usage or proof that a prior run depended on swap.'}
