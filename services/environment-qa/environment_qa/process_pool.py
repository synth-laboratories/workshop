"""Cooperating QA workers share a host/user-wide app-server capacity bound."""
import fcntl
import os
import tempfile
import time
from contextlib import contextmanager
from pathlib import Path

from .codex_executor import GateBlocked


class ProcessPool:
    def __init__(self, limit=4, root=None):
        if type(limit) is not int or limit < 1:
            raise ValueError("Capacity must be a positive integer")
        self.limit = limit
        self.root = Path(root or (Path(tempfile.gettempdir()) / f"workshop-qa-app-servers-{os.getuid()}"))
        self.root.mkdir(mode=0o700, parents=True, exist_ok=True)
        with (self.root / "capacity").open("a+") as config:
            fcntl.flock(config, fcntl.LOCK_EX)
            config.seek(0)
            current = config.read().strip()
            if current and current != str(limit):
                raise ValueError("All QA workers must use the same host app-server capacity")
            if not current:
                config.write(str(limit)); config.flush()

    @contextmanager
    def lease(self, key, timeout=240):
        deadline = time.monotonic() + timeout
        acquired = None
        while acquired is None:
            for slot in range(self.limit):
                stream = (self.root / f"slot-{slot}").open("a+")
                try:
                    fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    stream.close()
                    continue
                stream.seek(0); stream.truncate(); stream.write(f"{os.getpid()} {key}\n"); stream.flush()
                acquired = stream
                break
            if acquired is None:
                if time.monotonic() >= deadline:
                    raise GateBlocked("Host app-server capacity exhausted")
                time.sleep(min(.05, max(0, deadline-time.monotonic())))
        try:
            yield
        finally:
            fcntl.flock(acquired, fcntl.LOCK_UN)
            acquired.close()
