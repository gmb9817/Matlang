f = figure(40);
h1 = errorbar([1, 2, 3], [2, 1, 2], [0.2, 0.1, 0.3], 'horizontal', 'LineStyle', 'none');
hold on
h2 = errorbar([1, 2, 3], [2, 1, 2], [0.2, 0.1, 0.3], [0.4, 0.2, 0.5], [0.1, 0.2, 0.1], [0.2, 0.3, 0.2], 's-');
x_limits = xlim();
y_limits = ylim();
current = gcf();
