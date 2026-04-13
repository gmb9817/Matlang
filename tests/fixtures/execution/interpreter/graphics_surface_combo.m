z = [0, 1, 0; 1, 2, 1; 0, 1, 0];
f = figure(30);
surface_combo = surfc(z, [0.5, 1.0, 1.5]);
hold on
mesh_combo = meshc([1, 2, 3], [1, 2, 3], [1, 0, 1; 0, 1, 0; 1, 0, 1], [0.25, 0.75]);
scale = caxis();
title("Surface Combos");
current = gcf();
