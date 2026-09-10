"""Report resolved Python build-backend versions without building anything.

Case-11's reference names an unpinned setuptools 82 incompatibility. The
pipeline could not name it because it never measured it: the build probe ran a
reduced instrumentation package in an isolated environment and inferred a hook
failure instead. Resolution is a measurement, so make the measurement.
"""
import re

# Distributions that decide how a source build behaves. Kept explicit: a
# wildcard scan would report the whole environment and bury the signal.
BACKENDS = ('setuptools', 'wheel', 'pip', 'poetry', 'poetry-core', 'flit-core',
            'hatchling', 'meson-python', 'scikit-build-core', 'cython', 'numpy', 'pybind11')

COMMAND = (
    "python3 -c \"import json,sys\n"
    "try:\n"
    "    from importlib.metadata import version, PackageNotFoundError\n"
    "except Exception as exc:\n"
    "    print('QA_BUILD_BACKENDS ' + json.dumps({'error': type(exc).__name__})); raise SystemExit(0)\n"
    "found = {}\n"
    "for name in " + repr(list(BACKENDS)) + ":\n"
    "    try:\n"
    "        found[name] = version(name)\n"
    "    except PackageNotFoundError:\n"
    "        found[name] = None\n"
    "    except Exception as exc:\n"
    "        found[name] = 'unresolved:' + type(exc).__name__\n"
    "print('QA_BUILD_BACKENDS ' + json.dumps({'interpreter': sys.version.split()[0], 'resolved': found}))\""
)


def parse(stdout):
    """Return resolved backend versions; absent output stays explicitly unknown."""
    import json
    match = re.search(r'^QA_BUILD_BACKENDS (\{.*\})$', stdout or '', re.M)
    if not match:
        return {'resolved': {}, 'interpreter': None, 'measured': False,
                'notice': 'No build-backend measurement was produced; versions remain unknown, not absent.'}
    try:
        payload = json.loads(match[1])
    except ValueError:
        return {'resolved': {}, 'interpreter': None, 'measured': False,
                'notice': 'Build-backend measurement was unreadable; versions remain unknown, not absent.'}
    resolved = {k: v for k, v in (payload.get('resolved') or {}).items() if v}
    return {'resolved': resolved, 'interpreter': payload.get('interpreter'), 'measured': True,
            'installed': sorted(k for k, v in resolved.items() if isinstance(v, str) and not v.startswith('unresolved:')),
            'notice': ('Versions actually resolved in the task environment at probe time. A resolved version is a '
                       'measurement, not by itself an incompatibility; absence means the distribution was not '
                       'installed where this probe ran, which may differ from where the task build runs.')}


def summary(profile):
    """One line for the agent objective, naming versions rather than implying them."""
    resolved = (profile or {}).get('resolved') or {}
    if not resolved:
        return 'Resolved build-backend versions were not measured; do not assume any version.'
    named = ', '.join(f'{k} {v}' for k, v in sorted(resolved.items()))
    return ('Resolved build-backend versions in this environment: ' + named +
            '. Cite these measured versions when alleging a build-tool incompatibility; do not infer a version.')
