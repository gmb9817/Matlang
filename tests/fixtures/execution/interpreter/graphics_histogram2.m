f = figure(83);
hist2 = histogram2([0.2, 0.7, 1.4, 1.8], [0.1, 0.9, 1.1, 1.9], [0, 1, 2], [0, 1, 2]);
color_limits = caxis();
colorbar_handle = colorbar();
x_bounds = xlim();
y_bounds = ylim();
current = gcf();
