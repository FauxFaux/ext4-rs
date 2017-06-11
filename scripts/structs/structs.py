#!/usr/bin/env python3

import re

import sys

LINE = re.compile(r'\s*(\w+)\s+(\w+)\s*(?:\[(\w+)\])?;\s*(/\*.*\*/)?\s*')

LE16 = (2, 'read_le16')
LE32 = (4, 'read_le32')

TYPES = {
    '__le16': LE16,
    '__le32': LE32,
}


def load(lines):
    extra_size = None
    extra_size_counter = 0
    run = 0

    out = []

    for line in lines:
        line = line.strip()
        if not line:
            continue
        ma = LINE.match(line)
        if not ma:
            raise Exception('can\'t read: ' + line)

        (types, name, array_len, comment) = ma.groups()

        if 'extra_size' == types:
            extra_size = name
            extra_size_counter = 0
            continue

        if array_len:
            length = int(array_len)
            mapping_function = None
        else:
            (length, mapping_function) = TYPES[types]

        end_byte = run + length
        extra_size_counter += length

        out.append(('let {:17} = {}{:10}&data[0x{:02X}..0x{:02X}]{}{};'.format(
            name,
            'if {} < {:2} {{ None }} else {{ Some('.format(extra_size, extra_size_counter) if extra_size else '',
            (mapping_function + '(') if mapping_function else '',
            run, end_byte,
            ')' if mapping_function else ' ',
            ') }' if extra_size else ''
        ), comment))

        run += length

    # longest = max(len(x) for x, _ in out)
    # format = '{:' + str(longest) + '} {}'
    format = '{} {}'
    for line, comment in out:
        if comment:
            print(format.format(line, comment))
        else:
            print(line)



def main():
    with open(sys.argv[1]) as spec:
        load(spec)


if __name__ == '__main__':
    main()
