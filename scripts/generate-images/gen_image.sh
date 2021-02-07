#!/bin/sh
set -eu

run="$1"
output_name="$2"
size="$3"
opts="$4"


T=$(mktemp -p .)
D=$(mktemp -d .mounting.XXXXXXXXX)
trap 'rm "${T}"; rm -r "$D"' EXIT

truncate -s "$size" "$T"

# create a single partition that's as big as it can be:
printf 'n\n\n\n\n\nw\n' | fdisk "$T"

L=$(sudo losetup -P -f --show "$T")
sudo mkfs.ext4 -I 256 -O "$opts" "${L}p1"
sudo mount "${L}p1" "$D"

H=$(pwd)
(cd "$D" && sudo "$H/$run")


sudo umount "$D"
rm -r "$D"
sudo losetup -d "$L"
cp --sparse=always "$T" "$output_name"
rm "$T"
trap '' EXIT
