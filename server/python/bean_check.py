import io
import json
from sys import argv
from beancount import loader
from beancount.core import flags

entries, errors, options = loader.load_file(argv[1])

error_list = [{"file": e.source['filename'], "line": e.source['lineno'], "message": e.message} for e in errors]

flagged_entries = []

for entry in entries:
    if hasattr(entry, 'flag') and entry.flag == "!":
        flagged_entries.append({
            "file":    entry.meta['filename'],
            "line":    entry.meta['lineno'],
            "message": "Flagged Entry"
        })

print(json.dumps(error_list))
print(json.dumps(flagged_entries))
