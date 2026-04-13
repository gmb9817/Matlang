rgb = cat(3, [255, 0; 0, 255], [0, 255; 0, 255], [0, 0; 255, 255]);
f = figure(70);
img = imshow(rgb);
x_bounds = xlim();
y_bounds = ylim();
current = gcf();
