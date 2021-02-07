#!/bin/bash
set -eux

touch empty-file
mkdir empty-directory
mkdir -p a/deeply/nested/directory
mkdir -p a/multiple/entry/directory
mkdir -p home/faux
echo 'Hello, world!' > home/faux/hello.txt
chown 1000:1000 home/faux/hello.txt
truncate -s 10M sparse-file
mkfifo fifo-file
python3 -c 'import socket as s; sock = s.socket(s.AF_UNIX); sock.bind("sock-file")'
ln -s nonsense nonsense-symlink-file
ln sparse-file hardlink-file
mknod char-device c 1 3
mknod block-device b 7 6
mknod extremely-minor-device c 0 1023997
mknod extremely-major-device c 4093 0
touch single-xattr
setfacl -m u:nobody:r single-xattr

touch multiple-xattrs
setfacl -m u:mail:r multiple-xattrs
setcap cap_block_suspend+ep multiple-xattrs

touch -d '1902-03-04 05:06:07.890123456Z' old-file
touch -d '2039-12-31 23:59:59.999999999Z' next-file
touch -d '2345-06-07 08:09:10.111213141Z' future-file

# 28 non . entries
