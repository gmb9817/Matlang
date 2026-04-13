x = linspace(0, 2*pi, 5);
y = sin(x);

figure;
h = plot(x, y, 'LineWidth', 2, 'Color', [1, 0, 0], 'LineStyle', '--', 'Marker', 'o', 'MarkerSize', 7, 'DisplayName', 'Wave');
f = gcf();
w = get(h, 'LineWidth');
c = get(h, 'Color');
ls = get(h, 'LineStyle');
m = get(h, 'Marker');
ms = get(h, 'MarkerSize');
dn = get(h, 'DisplayName');
