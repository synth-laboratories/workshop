"""Deterministic protocol fixture. No weights; memory values are simulated."""
import uvicorn
from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.responses_api.backends import FakeBackend
cfg=LagunaConfig.from_env()
assert cfg.backend == 'mock'
app=build_app(cfg)
assert isinstance(app.state.responses_service.backend, FakeBackend)
app.state.synth_control.system_memory_bytes=64*1024**3
app.state.synth_control.available_memory_bytes=64*1024**3
uvicorn.run(app,host='127.0.0.1',port=17561)
