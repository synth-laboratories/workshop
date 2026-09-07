"""One pilot process owns this bounded resource pool; no container oversubscription."""
from contextlib import contextmanager
import threading
import time
import tomllib


def request(path):
    config = tomllib.loads((path/'task.toml').read_text())
    env = config.get('environment',{})
    memory = env.get('memory_mb')
    if memory is None:
        value = str(env.get('memory','2G')).upper()
        memory = float(value[:-1])*1024 if value.endswith('G') else float(value[:-1]) if value.endswith('M') else float(value)/1048576
    return float(env.get('cpus',1)),float(memory),config


class ResourcePool:
    def __init__(self,cpus,memory_mb,slots=4):
        self.cpus,self.memory_mb,self.slots = cpus,memory_mb,slots
        self.used_cpu=self.used_memory=self.active=0
        self.condition=threading.Condition()

    @contextmanager
    def acquire(self,path,store,run_id):
        cpu,memory,_ = request(path)
        if cpu>self.cpus or memory>self.memory_mb: raise ValueError('Task exceeds admitted host resource capacity')
        started=time.monotonic()
        with self.condition:
            while self.active>=self.slots or self.used_cpu+cpu>self.cpus or self.used_memory+memory>self.memory_mb:
                if store.get(run_id)['status'] in {'paused','cancelling','cancelled'}: raise ValueError('Resource wait stopped by run control')
                self.condition.wait(1)
            if store.get(run_id)['status'] in {'paused','cancelling','cancelled'}: raise ValueError('Resource dispatch stopped by run control')
            self.active+=1; self.used_cpu+=cpu; self.used_memory+=memory
        try:
            yield {'cpus':cpu,'memory_mb':memory,'queue_seconds':time.monotonic()-started}
        finally:
            with self.condition:
                self.active-=1; self.used_cpu-=cpu; self.used_memory-=memory
                self.condition.notify_all()


def trial_deadline(path,mode):
    _,_,config=request(path)
    return config.get('environment',{}).get('build_timeout_sec',600)+config.get('agent',{}).get('timeout_sec',900)+(2 if mode=='oracle-repeat' else 1)*config.get('verifier',{}).get('timeout_sec',900)+120
