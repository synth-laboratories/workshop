"""Recognize a timed-out package build without blocking unrelated installs."""
import re
import shlex


def timed_out_build(observation):
    if observation.get('return_code') != 124:
        return None
    text = (observation.get('stdout') or '') + '\n' + (observation.get('stderr') or '')
    started = re.findall(r'Building wheel for ([A-Za-z0-9_.-]+).*?: started', text)
    finished = set(re.findall(r'Building wheel for ([A-Za-z0-9_.-]+).*?: finished', text))
    return next((name.lower().replace('_', '-') for name in reversed(started) if name not in finished), None)


def repeats_build(command, blocked):
    for line in command.splitlines():
        if not re.search(r'\bpip\s+install\b', line):
            continue
        try:
            tokens = shlex.split(line.split('install', 1)[1], comments=True)
        except ValueError:
            continue
        for token in tokens:
            name = re.split(r'[<>=!~\[]', token, maxsplit=1)[0].lower().replace('_', '-')
            if name in blocked:
                return name
    return None
