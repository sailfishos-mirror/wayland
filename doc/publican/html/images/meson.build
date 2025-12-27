foreach src : files([
	'icon.svg',
	'wayland.png',
])
	name = fs.name(src)
	publican_inputs += fs.copyfile(
		name,
		install: true,
		install_dir: publican_install_prefix + '/html/images',
	)
endforeach

foreach src : files([
	'wayland-architecture.gv',
	'x-architecture.gv',
])
	input = fs.name(src)
	output = fs.stem(src) + '.png'

	publican_inputs += custom_target(
		input + '.png',
		command: [ dot, '-Tpng', '-o@OUTPUT@', '@INPUT@' ],
		input: input,
		output: output,
		install: true,
		install_dir: publican_install_prefix + '/html/images',
	)

	publican_image_maps += custom_target(
		input + '.map',
		command: [ dot, '-Tcmapx_np', '-o@OUTPUT@', '@INPUT@' ],
		input: input,
		output: fs.stem(src) + '.map',
	)
endforeach
