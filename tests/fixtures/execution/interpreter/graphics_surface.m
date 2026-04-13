z = [0, 1, 0; 1, 2, 1; 0, 1, 0];
f = figure(24);
surface_one = surf(z);
colormap("jet");
scale_one = caxis();
hold on
surface_two = surf([1, 2, 3], [1, 2, 3], [1, 0, 1; 0, 1, 0; 1, 0, 1]);
legend_handle = legend("Peak", "Checker");
cb = colorbar();
title("Surfaces");
current = gcf();
