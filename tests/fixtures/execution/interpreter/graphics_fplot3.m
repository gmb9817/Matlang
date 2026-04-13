f = figure(36);
curve = fplot3(@cos, @sin, @(t) t ./ pi, [0, 2*pi], 'm--', 'MeshDensity', 41, 'LineWidth', 1.5);
custom_view = view(50, 18);
queried_view = view();
title("FPlot3 Spiral");
current = gcf();
