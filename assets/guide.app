#!/usr/bin/env python3
"""The human manual. No cape required."""
import base64
import os
from pathlib import Path
import sys

topic = sys.argv[1] if len(sys.argv) > 1 else "welcome"
if topic not in {"welcome", "rice", "recovery", "source", "network", "storage"}:
    raise SystemExit("Topics: welcome, rice, recovery, source, network, storage")
root = Path("/system/docs")
figure = root / "figures" / ("storage.png" if topic in {"storage", "recovery"} else "workspace.png")
if os.environ.get("TERM", "").startswith("xterm-kitty") and figure.is_file():
    payload = base64.b64encode(figure.read_bytes()).decode()
    for index in range(0, len(payload), 4096):
        chunk = payload[index:index + 4096]
        more = int(index + 4096 < len(payload))
        control = f"a=T,f=100,c=75,r=12,q=2,m={more}" if index == 0 else f"m={more}"
        sys.stdout.write(f"\033_G{control};{chunk}\033\\")
    sys.stdout.write("\n" * 13)
text = (root / f"{topic}.md").read_text()
if sys.stdout.isatty() and len(text.splitlines()) > 35:
    import subprocess
    subprocess.run(["/usr/bin/less", "-R"], input=text, text=True, check=False)
else:
    print(text)
