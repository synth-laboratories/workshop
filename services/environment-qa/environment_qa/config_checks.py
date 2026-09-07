"""Task-configuration defects, anchored on task.toml itself.

Every other check anchors on the file that executes. Declared limits are a
defect surface of their own: the interaction between a declared bound and the
runtime that enforces it is invisible to a check anchored on the allocating
line. Findings here name task.toml so that configuration is reviewable.
"""
import re
import tomllib

from .review import finding

BYTES = {'K': 1024, 'M': 1024 ** 2, 'G': 1024 ** 3, 'T': 1024 ** 4}


def declared_bytes(value):
    """Parse a Harbor size string; unknown shapes stay unknown rather than guessed."""
    match = re.fullmatch(r'(\d+(?:\.\d+)?)\s*([KMGT])(?:i?B)?', str(value).strip(), re.I)
    return float(match[1]) * BYTES[match[2].upper()] if match else None


def line_of(text, *keys):
    """Locate a declaration so the finding points at the operator's own words."""
    for key in keys:
        match = re.search(r'^\s*' + re.escape(key) + r'\s*=.*$', text, re.M)
        if match:
            return text[:match.start()].count('\n') + 1, match[0].strip()
    return 1, (text.splitlines() or [''])[0]


def check_configuration(path, allocation_minimums=()):
    """allocation_minimums: (relative_path, line, minimum_bytes) already derived elsewhere."""
    config_path = path / 'task.toml'
    if not config_path.is_file():
        return []
    text = config_path.read_text(errors='replace')
    try:
        config = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return []
    environment = config.get('environment', {})
    findings = []

    budget = environment.get('memory_mb', 0) * 1024 ** 2 or declared_bytes(environment.get('memory', ''))
    worst = max(allocation_minimums, key=lambda item: item[2], default=None)
    if budget and worst:
        number, quote = line_of(text, 'memory', 'memory_mb')
        claim = (f'The declared {budget:g}-byte memory limit is below the {worst[2]:g}-byte lower-bound '
                 f'allocation in {worst[0]}, so whether this task passes is decided by the runtime memory '
                 f'policy rather than by the declared limit')
        item = finding('task_configuration', 'warning', claim, 'task.toml', number, quote,
                       'declared_memory_limit_enforcement_dependent')
        item.update(causal_claim=claim,
                    failure_condition=('The configuration declares a limit but not how it is enforced. A backing store '
                                       '(swap) or permissive accounting (overcommit) are distinct mechanisms and neither '
                                       'is declared here; each would let the over-limit workload pass on one runtime and '
                                       'fail on a stricter one.'),
                    affected_behavior='The same task can pass and fail across conforming backends with no configuration change.',
                    supporting_evidence=[{'path': 'task.toml', 'evidence': quote},
                                         {'path': worst[0], 'evidence': f'lower-bound allocation {worst[2]:g} bytes at line {worst[1]}'}],
                    limitations=['Which mechanism actually operates is not established by configuration alone and requires runtime measurement.'])
        findings.append(item)

    cpus = environment.get('cpus')
    if isinstance(cpus, int) and cpus == 1:
        for file in sorted(path.rglob('*.sh')):
            body = file.read_text(errors='replace')
            match = re.search(r'^.*\bmake\b[^\n]*\s-j\s*(\d+)?[^\n]*$', body, re.M)
            if not match:
                continue
            requested = match[1] or 'unbounded'
            number, quote = line_of(text, 'cpus')
            claim = (f'The environment declares cpus = 1 while {file.relative_to(path)} invokes a parallel build '
                     f'(-j {requested}), so build-ordering defects that only appear under real parallelism cannot '
                     f'surface here')
            item = finding('task_configuration', 'warning', claim, 'task.toml', number, quote,
                           'declared_cpu_limit_masks_parallel_build')
            item.update(causal_claim=claim,
                        failure_condition='A single available CPU serialises most make schedules, so a racing recipe pair may never interleave under this configuration.',
                        affected_behavior='A latent parallel-build race passes here and fails on a multi-CPU runtime.',
                        supporting_evidence=[{'path': 'task.toml', 'evidence': quote},
                                             {'path': str(file.relative_to(path)), 'evidence': match[0].strip()}])
            findings.append(item)
            break

    for section, key in (('verifier', 'timeout_sec'), ('agent', 'timeout_sec'), ('environment', 'build_timeout_sec')):
        value = config.get(section, {}).get(key)
        if isinstance(value, (int, float)) and value <= 0:
            number, quote = line_of(text, key)
            claim = f'{section}.{key} is declared as {value:g}, which cannot bound execution'
            item = finding('task_configuration', 'warning', claim, 'task.toml', number, quote,
                           'nonpositive_declared_timeout_' + section)
            item.update(causal_claim=claim, failure_condition='The declared bound is not a positive duration.',
                        affected_behavior='Execution is effectively unbounded or immediately terminated.')
            findings.append(item)
    return findings
