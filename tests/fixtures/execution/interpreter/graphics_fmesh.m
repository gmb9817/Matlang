f = figure(225);
h = fmesh(@(x, y) x.^2 - y.^2, [-1, 1, -1, 1], 'MeshDensity', 15);
current = gcf();
