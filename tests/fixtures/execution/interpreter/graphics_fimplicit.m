f = figure(228);
h = fimplicit(@(x, y) x.^2 + y.^2 - 1, [-2, 2, -2, 2], 'MeshDensity', 19);
current = gcf();
