#!/usr/bin/env python3
import os
import sys

print("Let there be light.")
print("Hello from a Python application inside Glyph.")
print("Working directory:", os.getcwd())
print("Arguments:", repr(sys.argv[1:]))
print("Your files:", ", ".join(sorted(os.listdir("/home"))))
