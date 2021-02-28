#!/usr/bin/env python3

import os
import re
from typing import Iterable

root_dir = os.path.dirname(os.path.realpath(__file__)) + '/'

SPEC_LINE = re.compile(r'\s*(\w+)\s+(\w+)\s*(?:\[(\w+)\])?;\s*(/\*.*\*/)?\s*')

TYPES = {
    '__be16': (2, 'read_be16', 'u16'),
    '__le16': (2, 'read_le16', 'u16'),
    '__le32': (4, 'read_le32', 'u32'),
    '__lei32': (4, 'read_lei32', 'i32'),
}


def load(struct_name: str, lines: Iterable[str], extra_is_arg: bool, extra_field: str, core_size: int):
    run = 0

    fields = []

    for line in lines:
        line = line.strip()
        if not line:
            continue
        ma = SPEC_LINE.match(line)
        if not ma:
            raise Exception('can\'t read: ' + line)

        (types, name, array_len, comment) = ma.groups()

        if array_len:
            length = int(array_len)
            mapping_function = None
            kind = None
        else:
            (length, mapping_function, kind) = TYPES[types]

        fields.append((name, kind, length, mapping_function, run, comment))

        run += length

    ret = f'pub struct Raw{struct_name} {{\n'

    def is_extra():
        return start >= core_size and not name == extra_field

    for (name, kind, length, conv, start, comment) in fields:
        if comment:
            ret += f'    {comment}\n'
        ret += f'    pub {name}: '
        if is_extra():
            ret += 'Option<'
        if kind:
            ret += kind
        else:
            ret += f'[u8; {length}]'
        if is_extra():
            ret += '>'
        ret += ',\n'

    ret += '}\n\n'

    def read_field():
        s = ''
        if is_extra():
            s += f'if {extra_field} >= {start - core_size + length} {{ Some('
        if conv:
            s += f'{conv}(&data[0x{start:02x}..])'
        else:
            s += f'data[0x{start:02x}..0x{start + length:02x}].try_into().expect("sliced")'
        if is_extra():
            s += ') } else { None }'
        return s

    ret += f'impl Raw{struct_name} {{\n'
    ret += '    pub fn from_slice(data: &[u8]'
    if extra_is_arg:
        ret += f', {extra_field}: u16'
    ret += ') -> Self {\n'
    if not extra_is_arg:
        (name, kind, length, conv, start, comment) = next(x for x in fields if x[0] == extra_field)
        ret += f'        let {extra_field} = {conv}(&data[0x{start:02x}..]);\n'

    ret += '        Self {\n'
    for (name, kind, length, conv, start, comment) in fields:
        ret += f'            {name}'
        if name != extra_field:
            ret += f': {read_field()}'
        ret += ',\n'

    ret += '        }\n    }\n}\n\n'

    return ret


def main():
    ret = ''

    ret += 'use std::convert::TryInto;\n'
    ret += '\n'
    ret += 'use crate::read_be16;\n'
    ret += 'use crate::read_le16;\n'
    ret += 'use crate::read_le32;\n'
    ret += 'use crate::read_lei32;\n'
    ret += '\n'

    for (name, f, extra_is_arg, extra_field, core_size) in [
        # TODO: kinda optional
        ('Inode', 'inode', False, 'i_extra_isize', 128),
        ('BlockGroup', 'block-group', True, 's_desc_size', 32),
    ]:
        with open(root_dir + f + '.spec') as spec:
            ret += load(name, spec, extra_is_arg, extra_field, core_size)

    print(ret)


if __name__ == '__main__':
    main()
