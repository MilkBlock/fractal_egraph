"""Compatibility module; implementation is in eggfuse.compiler.targets.metal."""
import sys
from pathlib import Path
_ROOT=Path(__file__).resolve().parents[2]
if str(_ROOT) not in sys.path:sys.path.insert(0,str(_ROOT))
from eggfuse.compiler.targets.metal import matrix_sat as _implementation
sys.modules[__name__] = _implementation
