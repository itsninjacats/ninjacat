#!/usr/bin/env python3
"""Exports one event of every kind a lab run produced into Fixtures/real/<name>-<track>.json.

Usage: export.py <name> <from> <to>, the times in UTC as ClickHouse writes them.
"""
import json, os, subprocess, sys, collections

name, start, end = sys.argv[1], sys.argv[2], sys.argv[3]
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'api', 'tests', 'Intake.Tests', 'Fixtures', 'real')

def ch(query):
    return subprocess.run(['docker', 'exec', 'ninjacat-clickhouse', 'clickhouse-client', '-u', 'ninjacat', '--password', 'ninjacat',
                           '-d', 'ninjacat_lab', '-q', query], capture_output=True, text=True, check=True).stdout

window = f"received_at >= '{start}' AND received_at < '{end}'"
# The smallest event of each kind; for a plan, one with a definition and one without when both came.
rows = ch(f"""
SELECT track, argMin(event, length(event))
FROM dbm_events
WHERE {window}
GROUP BY track, dbm_type, kind, JSONExtractString(event, 'name'), JSONType(event, 'db', 'plan', 'definition')
ORDER BY track, dbm_type, kind
FORMAT JSONEachRow""")
by_track = collections.OrderedDict()
for line in rows.splitlines():
    record = json.loads(line)
    by_track.setdefault(record['track'], []).append(record['argMin(event, length(event))'])
for track, events in by_track.items():
    path = f'{out}/{name}-{track}.json'
    with open(path, 'w') as f:
        f.write('[' + ','.join(events) + ']')
    kinds = [(e.get('dbm_type') or e.get('kind') or e.get('name') or '-') for e in map(json.loads, events)]
    print(f'{name}-{track}.json', len(events), sum(len(e) for e in events), kinds)
