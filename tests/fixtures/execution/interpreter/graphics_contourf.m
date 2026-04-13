z = [1, 2, 3; 2, 4, 2; 1, 2, 1];
f = figure(26);
filled = contourf(z, [1.5, 2.5, 3.5]);
colormap("jet");
scale = caxis();
cb = colorbar();
hold on
lines = contour([1, 2, 3], [1, 2, 3], z, [1.5, 2.5, 3.5]);
legend_handle = legend("Filled", "Lines");
title("Filled Contours");
current = gcf();
