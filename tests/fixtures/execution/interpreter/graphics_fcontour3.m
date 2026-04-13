f = figure(227);
h = fcontour3(@(x, y) x.^2 - y.^2, [-1, 1, -1, 1], 'MeshDensity', 15);
current = gcf();
