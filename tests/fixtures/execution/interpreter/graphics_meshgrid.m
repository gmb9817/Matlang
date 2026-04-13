[xg, yg] = meshgrid([-1, 0, 1], [-1, 0, 1]);
z = xg + yg;
f = figure(36);
surface = surf(xg, yg, z);
title("Meshgrid Surface");
current = gcf();
