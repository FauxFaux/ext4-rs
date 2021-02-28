#!/usr/bin/env python3

import re
import sys
from typing import Iterable

SPEC_LINE = re.compile(r'\s*(\w+)\s+(\w+)\s*(?:\[(\w+)\])?;\s*(/\*.*\*/)?\s*')

TYPES = {
    '__le16': (2, 'read_le16', 'u16'),
    '__le32': (4, 'read_le32', 'u32'),
    '__lei32': (4, 'read_lei32', 'i32'),
}


def load(struct_name: str, lines: Iterable[str]):
    extra_size = False
    extra_size_counter = None
    run = 0

    fields = []
    lift = []

    for line in lines:
        line = line.strip()
        if not line:
            continue
        ma = SPEC_LINE.match(line)
        if not ma:
            raise Exception('can\'t read: ' + line)

        (types, name, array_len, comment) = ma.groups()

        if 'extra_size' == types:
            extra_size = True
            extra_size_counter = 0
            lift.append(name)
            continue

        if array_len:
            length = int(array_len)
            mapping_function = None
            kind = None
        else:
            (length, mapping_function, kind) = TYPES[types]

        if extra_size:
            extra_size_counter += length

        fields.append((name, kind, length, mapping_function, run, comment, extra_size_counter))

        run += length

    ret = f'pub struct Raw{struct_name} {{\n'

    for (name, kind, length, conv, start, comment, extra) in fields:
        if comment:
            ret += f'    {comment}\n'
        ret += f'    pub {name}: '
        if extra:
            ret += 'Option<'
        if kind:
            ret += kind
        else:
            ret += f'[u8; {length}]'
        if extra:
            ret += '>'
        ret += ',\n'

    ret += '}\n\n'

    ret += f'impl Raw{struct_name} {{\n'
    ret += '    pub fn from_slice(data: &[u8]) -> Self {\n'
    for (name, kind, length, conv, start, comment, extra) in fields:
        if name not in lift:
            continue
        ret += f'        let {name} = {read_field(conv, start, length, extra)};\n'
    if lift:
        ret += '\n'

    ret += '        Self {\n'
    for (name, kind, length, conv, start, comment, extra) in fields:
        ret += f'            {name}'
        if name not in lift:
            ret += f': {read_field(conv, start, length, extra)}'
        ret += ',\n'

    ret += '        }\n    }\n}\n\n'

    return ret


def read_field(conv, start, length, extra):
    ret = ''
    if extra:
        ret += f'if i_extra_isize >= {extra} {{ Some('
    if conv:
        ret += f'{conv}(&data[0x{start:02x}..])'
    else:
        ret += f'data[0x{start:02x}..0x{start + length:02x}].try_into().expect("sliced")'
    if extra:
        ret += ') } else { None }'
    return ret


def main():
    ret = ''

    ret += 'use std::convert::TryInto;\n'
    ret += '\n'
    ret += 'use crate::read_le16;\n'
    ret += 'use crate::read_le32;\n'
    ret += 'use crate::read_lei32;\n'
    ret += '\n'

    with open(sys.argv[1]) as spec:
        ret += load('Inode', spec)

    print(ret)


if __name__ == '__main__':
    main()
