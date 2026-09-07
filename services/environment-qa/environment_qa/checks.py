"""Deterministic package and verifier checks, separated from semantic judgment."""
import ast
import re
import tomllib
from .review import finding, static_review


def check_task(path):
    config = tomllib.loads((path/"task.toml").read_text())
    findings = []
    checks = []
    required = ["instruction.md","task.toml","environment/Dockerfile","solution/solve.sh","tests/test.sh"]
    missing = [p for p in required if not (path/p).is_file()]
    checks.append({"id":"required_components","passed":not missing,"missing":missing})
    for section in ("agent","verifier"):
        timeout = config.get(section,{}).get("timeout_sec",900)
        valid = type(timeout) in {int,float} and 0 < timeout <= 28800
        checks.append({"id":section+"_timeout","passed":valid,"value":timeout})
    for file in sorted((path/"tests").rglob("*.py")):
        text=file.read_text(errors="replace")
        try: ast.parse(text)
        except SyntaxError as exc:
            findings.append(finding("verifier_validity","blocking","Verifier Python does not parse",str(file.relative_to(path)),exc.lineno or 1,str(exc),"invalid_python"))
            checks.append({"id":"python_parse","path":str(file.relative_to(path)),"passed":False})
    separate = config.get("verifier",{}).get("environment_mode") == "separate"
    if separate:
        checks.append({"id":"separate_verifier_image","passed":(path/"tests/Dockerfile").is_file()})
        checks.append({"id":"artifact_declaration","passed":isinstance(config.get("artifacts"),list) and bool(config["artifacts"])})
    dockerfile = path/"environment/Dockerfile"
    if dockerfile.is_file():
        text = dockerfile.read_text()
        for line_no,line in enumerate(text.splitlines(),1):
            if re.match(r"\s*(COPY|ADD)\s",line,re.I) and re.search(r"(?:^|[\s\"/])(solution/|tests/)",line):
                findings.append(finding("solution_leakage","warning","Agent image copies potentially privileged solution or test artifacts","environment/Dockerfile",line_no,line,"privileged_artifact_copy"))
            if re.search(r"FROM\s+--platform=",line,re.I):
                findings.append(finding("reproducibility","warning","Image build pins a processor architecture","environment/Dockerfile",line_no,line,"architecture_pinned"))
    shell = path/"tests/test.sh"
    if separate and shell.is_file():
        for line_no,line in enumerate(shell.read_text().splitlines(),1):
            if re.search(r"(?:curl|wget|git\s+clone)\s+.*https?://",line):
                findings.append(finding("reproducibility","warning","Separate verifier fetches external inputs during grading","tests/test.sh",line_no,line,"trial_time_external_fetch"))
    for f in static_review(path)["findings"]:
        if f["mechanism"] != "invalid_python": findings.append(f)
    failed = [c["id"] for c in checks if not c["passed"]]
    return {"gate_status":"failed" if failed else "succeeded","findings":findings,"checks":checks,
            "limitations":["Structural checks failed: "+", ".join(failed)] if failed else [],
            "coverage":{"static":"package, timeout, syntax, isolation declarations, image visibility and verifier network checks",
                        "semantic":"delegated to independent specification and verifier reviewers"}}
