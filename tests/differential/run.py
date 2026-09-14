"""Explicit CPython oracle; the Tonic runtime itself never starts Python."""
from pathlib import Path
import os
import random
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
TONIC = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'target/debug/tonic'
CASES = [
    (ROOT / 'examples/fib.tonic').read_text(),
    (ROOT / 'examples/control_flow.tonic').read_text(),
    (ROOT / 'examples/closures.tonic').read_text(),
    (ROOT / 'examples/classes.tonic').read_text(),
    'def fact(n):\n    if n<2:\n        return 1\n    return n*fact(n-1)\nprint(fact(40))\n',
    'a,b=1,2\na,b=b,a+b\nprint(a,b)\nprint([1,2]+[3], (1,)+(2,))\n',
    "print(len('é字'), 'é字'[-1], [1,2]<[1,3], (1,2)==(1,2))\n",
    'print(0 and missing, 3 or missing, False==0, True+True)\n',
    'def tick(n):\n    print(n)\n    return n\nprint(3<tick(2)<tick(1))\n',
    'for i in range(7,0,-2):\n    if i==3:\n        break\n    print(i)\nelse:\n    print(99)\n',
    'x=3\ndef f():\n    return x\nx=9\nprint(f())\n',
    'print(9007199254740993 > 9007199254740992.0, 9007199254740993 == 9007199254740992.0)\n',
    'print(abs(-0.0), -1.0 % 0.5, 1.0 // 0.1)\n',
]
# Seeded arithmetic oracle exercises signs, immediate boundaries and big integers.
rng = random.Random(42)
for _ in range(120):
    a = rng.choice([rng.randint(-10**8, 10**8), rng.randint(-(1 << 90), 1 << 90)])
    b = rng.randint(-10000, 10000) or 1
    CASES.append(f'print({a}+({b}), {a}-({b}), {a}*({b}), {a}//({b}), {a}%({b}))\n')
CASES.append('a=[1]\nb=a\na += (2,3)\na += a\nprint(a,b)\na += (a,)\nprint(a)\n')
for _ in range(80):
    a = rng.randint(-(1 << 180), 1 << 180)
    b = rng.randint(1, 1 << 170)
    CASES.append(f'print({a}/({b}))\n')
for a, b in [(10**400, 10**400), (1, 2**1075), (3, 2**1075), (1, 2**1074), (0, -10**400)]:
    CASES.append(f'print({a}/({b}))\n')
ERRORS = [
    ('print(1//0)', 'ZeroDivisionError'),
    ('print(missing)', 'NameError'),
    ('x=1\ndef f():\n    print(x)\n    x=2\nf()', 'UnboundLocalError'),
    ('a,b=[1]', 'ValueError'),
    ('print([1][4])', 'IndexError'),
    ('range(1,4,0)', 'ValueError'),
    ('def f(a):\n    return a\nf()', 'TypeError'),
    ("1+'x'", 'TypeError'),
]

def run(binary, source, tonic=False):
    options = ['--fuel', '1000000'] if tonic else []
    if tonic and os.getenv('TONIC_JIT') == '1':
        # Differential fuel intentionally keeps JIT disabled: native loops do
        # not have fuel polls yet. JIT runs use the same process timeout instead.
        options = ['--jit']
    if tonic and 'TONIC_GC_EVERY' in os.environ:
        options += ['--gc-every', os.environ['TONIC_GC_EVERY']]
    args = [str(binary)] + options + ['-c', source]
    return subprocess.run(args, capture_output=True, text=True, timeout=10)

from scopes_calls import CASES as FEATURE_CASES, ERRORS as FEATURE_ERRORS
CASES.extend(FEATURE_CASES)
ERRORS.extend(FEATURE_ERRORS)
from classes import CASES as CLASS_CASES, ERRORS as CLASS_ERRORS
CASES.extend(CLASS_CASES)
ERRORS.extend(CLASS_ERRORS)
from slices import CASES as SLICE_CASES, ERRORS as SLICE_ERRORS
CASES.extend(SLICE_CASES)
ERRORS.extend(SLICE_ERRORS)

for number, source in enumerate(CASES):
    py, tonic = run(sys.executable, source), run(TONIC, source, True)
    assert py.returncode == tonic.returncode == 0, (number, source, py.stderr, tonic.stderr)
    assert py.stdout == tonic.stdout, (number, source, py.stdout, tonic.stdout)
for source, kind in ERRORS:
    py, tonic = run(sys.executable, source), run(TONIC, source, True)
    assert py.returncode != 0 and tonic.returncode != 0, (source, py.stdout, tonic.stdout)
    assert py.stdout == tonic.stdout, (source, py.stdout, tonic.stdout)
    assert kind + ':' in py.stderr and tonic.stderr.startswith(kind + ':'), (source, py.stderr, tonic.stderr)
print(f'PASS: {len(CASES)} output cases + {len(ERRORS)} exception cases; '
      f'Python {sys.version.split()[0]}; GC interval {os.getenv("TONIC_GC_EVERY", "default")}; '
      f'JIT {os.getenv("TONIC_JIT", "0")}')
