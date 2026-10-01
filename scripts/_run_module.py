"""Private launcher for owned tool modules; preserve cwd and relative arguments."""

from pathlib import Path
import runpy
import sys


def main():
    # A caller may have an unrelated scripts package. Select this checkout
    # before importing the owned module without changing cwd or PYTHONPATH.
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    module = sys.argv.pop(1)
    runpy.run_module(module, run_name="__main__", alter_sys=True)


if __name__ == "__main__":
    main()
