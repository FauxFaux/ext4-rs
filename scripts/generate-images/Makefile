all: images.tgz

%.img: img-%.sh gen_image.sh
	./gen_image.sh $< $@

images.tgz: all-types.img
	tar -zcf $@ --sparse $^

clean:
	rm images.tgz *.img
