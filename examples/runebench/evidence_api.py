"""Loopback demo adapter for Containers' canonical annotation evidence store."""
import json
import os
import secrets
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

from synth_containers.tracing.annotation_store import AnnotationStore
from synth_containers.tracing.validation.rehydrate import rehydrate_trace, evidence_bundle_from_payload
from synth_containers.tracing.projections.visual import visual_from_sealed

STORE = AnnotationStore(HERE / 'annotation-evidence')

def capability():
    path = HERE / '.annotation-capability'
    if not path.exists():
        try:
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, 'w') as f: f.write(secrets.token_urlsafe(32))
        except FileExistsError: pass
    return path.read_text().strip()

def sources(run):
    if not run or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-' for c in run):
        raise ValueError('Invalid run')
    directory = HERE / 'trace-v5'
    document = rehydrate_trace(json.loads((directory / f'{run}.trace.json').read_text()))
    base = evidence_bundle_from_payload(json.loads((directory / f'{run}.evidence.json').read_text()))
    return document, base

def evidence(run, request=None):
    document, base = sources(run)
    if request is None:
        bundle = STORE.load(document, base)
    else:
        bundle, _ = STORE.append(document, base, request)
    return {'projection': visual_from_sealed(document, bundle).to_dict(), 'evidence_digest': bundle.content_digest}
