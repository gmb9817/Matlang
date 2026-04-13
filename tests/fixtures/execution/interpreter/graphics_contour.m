z = [1, 2, 3; 2, 4, 2; 1, 2, 1];
f = figure(23);
default_lines = contour(z);
equal_axis = axis("equal");
hold on
explicit_lines = contour([1, 2, 3], [1, 2, 3], z, [1.5, 2.5, 3.5]);
legend_handle = legend("Auto", "Explicit");
title("Contours");
limits = axis();
current = gcf();
