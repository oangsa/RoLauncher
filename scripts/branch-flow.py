"""Check PR admission using read-only GitHub evidence; print reviewable scope report."""
import json
import os
from pathlib import Path
from hotfix_policy import check_branch_flow
from release_promotion import github_api

if __name__ == '__main__':
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text(encoding='utf-8'))
    print(check_branch_flow(event, github_api(os.environ.get('GITHUB_TOKEN', ''))))
