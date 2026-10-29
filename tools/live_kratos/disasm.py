"""Disassemble the code the probe saved from GoWR.exe (code_<rva>.bin, named by the function's RVA), marking the
addresses the probe's log names (writers and callers). Usage: python3 -I disasm.py code_498a40.bin [rva ...]
Needs capstone (pip install capstone)."""
import os, sys
from capstone import Cs, CS_ARCH_X86, CS_MODE_64


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    path = sys.argv[1]
    start = int(os.path.basename(path).split('_')[1].split('.')[0], 16)
    marks = {int(a, 16) for a in sys.argv[2:]}
    code = open(path, 'rb').read()
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    for i in md.disasm(code, start):
        mark = '  <==' if i.address in marks or i.address + i.size in marks else ''
        print(f'GoWR.exe+{i.address:#x}  {i.bytes.hex():<24} {i.mnemonic} {i.op_str}{mark}')


if __name__ == '__main__':
    main()
