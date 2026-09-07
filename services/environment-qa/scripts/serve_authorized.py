"""Start the local QA service using an explicit project env file; no Keychain."""
import argparse
import os
from pathlib import Path
from dotenv import dotenv_values
from environment_qa.server import serve


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--store",type=Path,required=True)
    parser.add_argument("--env-file",type=Path,required=True)
    parser.add_argument("--codex",required=True)
    parser.add_argument("--task-root",type=Path,action="append",required=True)
    parser.add_argument("--port",type=int,default=7338)
    parser.add_argument("--provider-budget-usd",type=float,required=True)
    parser.add_argument("--allowance-id",required=True)
    parser.add_argument("--operator-token",
                        help="Secret a client must send as X-QA-Operator for a decision to be "
                             "recorded as a verified human one. Give it to the person reviewing "
                             "and not to an automated harness; without it every decision is "
                             "recorded as an unverified client claim.")
    args=parser.parse_args()
    values=dotenv_values(args.env_file)
    key=values.get("OPENROUTER_API_KEY") or values.get("QA_PROVIDER_KEY")
    if not key: raise SystemExit("No OpenRouter key in the explicitly selected env file")
    os.environ.update(OPENROUTER_API_KEY=key, QA_CODEX_APP_SERVER=args.codex,
                      QA_INPUT_USD_PER_MILLION="0.2",QA_OUTPUT_USD_PER_MILLION="1.2",QA_TOKEN_BUDGET="")
    serve(args.store,args.task_root,args.port,args.provider_budget_usd,args.allowance_id,
          operator_token=args.operator_token)


if __name__=="__main__": main()
