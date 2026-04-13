f = figure(73);

top = subplot(211);
img = image([10, 30], [5, 15], [1, 2; 3, 4]);
x_before = get(img, 'XData');
y_before = get(img, 'YData');
limits_before = axis();
set(img, 'XData', [20, 50], 'YData', [0, 30]);
x_after = get(img, 'XData');
y_after = get(img, 'YData');
limits_after = axis("tight");

bottom = subplot(212);
heat = imagesc([100, 200, 400], [2, 8], [0, 1, 2; 3, 4, 5]);
heat_x = get(heat, 'XData');
heat_y = get(heat, 'YData');
heat_limits = axis("tight");

current = gcf();
