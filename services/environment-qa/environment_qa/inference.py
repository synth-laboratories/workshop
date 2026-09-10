"""Atomic run-wide provider admission; preserve usage even on invalid model JSON."""
import json
import os
import uuid
import math
import urllib.request
import urllib.error
from . import accounting
from .core import digest
from .review import ai_request


def validate_shape(value, schema, path='$'):
    kind = schema.get("type")
    if kind == "object":
        if not isinstance(value,dict) or not set(schema.get("required",[])) <= value.keys(): raise ValueError("Missing required object fields")
        if schema.get("additionalProperties") is False and not value.keys() <= schema.get("properties",{}).keys(): raise ValueError("Unexpected object fields")
        for key, sub in schema.get("properties",{}).items():
            if key in value: validate_shape(value[key],sub,path+'.'+key)
    elif kind == "array":
        if not isinstance(value,list): raise ValueError(f"Invalid array at {path}: expected a list")
        if len(value)>schema.get("maxItems",1000):
            raise ValueError(f"Invalid array at {path}: maximum {schema.get('maxItems',1000)} items, received {len(value)}")
        for index,item in enumerate(value): validate_shape(item,schema["items"],f'{path}[{index}]')
    elif kind == "string" and not isinstance(value,str): raise ValueError("Expected string")
    elif kind == "boolean" and type(value) is not bool: raise ValueError("Expected boolean")
    if "enum" in schema and value not in schema["enum"]: raise ValueError("Unknown enum value")


def request_json(store, run_id, gate_id, messages, max_tokens=4096, attempt_token=None, response_schema=None, repair_attempt=0):
    run = store.get(run_id)
    from .bundles import verified_path
    endpoint, key, body, _, rates, _ = ai_request(verified_path(store, run["bundle"]), run["policy"])
    if body["model"] != run["policy"]["pipeline"]["model"]: raise ValueError("Provider model differs from pinned policy")
    policy=run['policy']['pipeline']
    current_gate=next(g for g in run['gates'] if g['id']==gate_id)
    command_step=current_gate['executor'] in {'targeted_trial','agent_trial'}
    effort=policy.get('agent_reasoning_effort') if command_step else policy.get('reasoning_effort')
    tool_name='execute_qa_step' if command_step else 'submit_qa_result'
    completion_limit=max_tokens+(4096 if effort in {'high','xhigh','max'} else 0)
    body.update(messages=messages, max_completion_tokens=completion_limit)
    if effort is not None: body['reasoning_effort']=effort
    if response_schema is not None:
        body.pop("response_format",None)
        description='Execute the next concrete shell command in the isolated QA container. The host runs command immediately and returns its observation. This is an execution step, not a final report. Empty command with done=true finishes the attempt.' if command_step else 'Submit exactly one structured QA result.'
        body["tools"] = [{"type":"function","function":{"name":tool_name,"description":description,"strict":True,"parameters":response_schema}}]
        body["tool_choice"] = {"type":"function","function":{"name":tool_name}}
        body["parallel_tool_calls"] = False
    prompt_bytes = len(json.dumps(body, ensure_ascii=False).encode())
    if prompt_bytes > 240_000: raise ValueError("Evidence context exceeds admitted token-rate tier; summarize inputs explicitly")
    reserve = accounting.reservation(prompt_bytes, completion_limit, rates)
    call_id = accounting.admit(store, run_id, gate_id, attempt_token or os.environ.get("QA_GATE_ATTEMPT"),
                               reserve, digest(body))
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs): raise ValueError("Provider redirect rejected")
    req = urllib.request.Request(endpoint, data=json.dumps(body).encode(), headers={"Content-Type": "application/json", "Authorization": "Bearer "+key})
    try:
        with urllib.request.build_opener(NoRedirect).open(req, timeout=120) as response:
            data = json.loads(response.read(2_000_001))
    except Exception as error:
        status=error.code if isinstance(error,urllib.error.HTTPError) else None
        detail=''
        if status is not None:
            try:
                payload=json.loads(error.read(4096))
                item=payload.get('error',{})
                detail=str(item.get('message',''))[:1000] if isinstance(item,dict) else str(item)[:1000]
            except Exception:pass
        detail=detail.replace(key,'[REDACTED]')
        failure={'type':type(error).__name__,'http_status':status,'message':detail}
        accounting.record_failure(store, run_id, call_id, failure)
        raise ValueError('Provider request failed'+(f' (HTTP {status}: {detail})' if status else '')+'; reservation retained, no automatic retry') from None
    usage = data.get("usage", {})
    actual = None
    if all(type(usage.get(k)) is int and usage[k] >= 0 for k in ("prompt_tokens", "completion_tokens")):
        actual = (usage["prompt_tokens"]*rates[0]+usage["completion_tokens"]*rates[1])/1_000_000
    accounting.settle(store, run_id, call_id, usage, actual)
    message = data["choices"][0]["message"]
    content = message.get("content")
    # Store response separately from its parse result, so malformed JSON is auditable.
    artifact = store.root / "responses" / run_id / (call_id+".json")
    artifact.parent.mkdir(parents=True, exist_ok=True)
    artifact.write_text(json.dumps({"content": content,"message":message, "usage": usage, "model": body["model"],
                                   'reported_model':data.get('model'),'requested_reasoning_effort':effort,
                                   "finish_reason": data["choices"][0].get("finish_reason")}, indent=2))
    if any(name in str(data.get('model','')).lower() for name in ('claude','anthropic')):
        raise ValueError('Provider returned a prohibited model; response retained, no retry')
    if data.get('model') and data['model']!=body['model']:
        raise ValueError('Provider returned a different model than the pinned policy; response retained, no retry')
    try:
        if response_schema is not None:
            calls = message.get("tool_calls",[])
            if len(calls) != 1 or calls[0].get("function",{}).get("name") != tool_name:
                raise ValueError("Exactly one structured result tool call required")
            content = calls[0]["function"]["arguments"]
        parsed = json.loads(content)
        if not isinstance(parsed, dict): raise ValueError("Model JSON must be an object")
        if response_schema is not None: validate_shape(parsed,response_schema)
    except (ValueError,TypeError):
        if response_schema is not None and repair_attempt == 0:
            repair = messages + [{"role":"user","content":"The preceding response failed output validation. Return exactly ONE JSON object conforming to the response schema, including every required field. No prose or second object. Required schema: "+json.dumps(response_schema)}]
            return request_json(store,run_id,gate_id,repair,max_tokens,attempt_token,response_schema,1)
        raise ValueError("Model response failed JSON/schema validation after bounded repair") from None
    return parsed
