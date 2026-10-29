"""Compatibility import; implementation moved to eggfuse.compiler.verify."""
import sys
from pathlib import Path
_ROOT=Path(__file__).resolve().parents[2]
if str(_ROOT) not in sys.path:sys.path.insert(0,str(_ROOT))
from eggfuse.compiler.verify import *
