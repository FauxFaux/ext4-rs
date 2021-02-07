all: images.tgz

GEN=./gen_image.sh

%.img: img-%.sh $(GEN)
	$(GEN) $< $@ 8M ''

all-types-32.img: img-all-types.sh $(GEN)
	$(GEN) $< $@ 8M ^64bit

all-types-tiny.img: img-all-types.sh $(GEN)
	$(GEN) $< $@ 1M ''

all-types-big.img: img-all-types.sh $(GEN)
	$(GEN) $< $@ 80G ''

all-types-big-32.img: img-all-types.sh $(GEN)
	$(GEN) $< $@ 80G ^64bit

images.tgz: all-types.img all-types-32.img all-types-tiny.img all-types-big.img all-types-big-32.img
	tar -zcf $@ --sparse $^

clean:
	rm -f images.tgz *.img
