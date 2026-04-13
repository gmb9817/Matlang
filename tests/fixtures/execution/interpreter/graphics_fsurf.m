f = figure(224);
h = fsurf(@(x, y) sin(x) + cos(y), [-2, 2, -3, 3], 'MeshDensity', 17);
current = gcf();
