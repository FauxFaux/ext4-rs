all: all-types.img

%.img: img-%.sh gen_image.sh
	./gen_image.sh $< $@
