f = figure(74);

left = subplot(121);
rgb_left = cat(3, [1, 0; 0, 1], [0, 1; 0, 1], [0, 0; 1, 1]);
img = image(rgb_left);
img_type = get(img, 'Type');
img_cdata = get(img, 'CData');
img_axis = axis("tight");

right = subplot(122);
rgb_right = cat(3, [255, 128; 0, 64], [0, 255; 128, 64], [255, 0; 64, 128]);
heat = imagesc(rgb_right);
heat_cdata = get(heat, 'CData');
heat_axis = axis("tight");

current = gcf();
