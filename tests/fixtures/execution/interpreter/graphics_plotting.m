x = [0, 1, 2, 3];
y = [0, 1, 4, 9];

f = figure();
line1 = plot(x, y);
title("Quadratic");
xlabel('Input');
ylabel("Output");
hold on
line2 = plot(x, [0, 1, 8, 27]);
x_bounds = xlim([0, 3]);
y_bounds = ylim([0, 27]);
current = gcf();
clf(f);
line3 = plot([1, 2], [3, 5]);
limits_after_clf = xlim();

figure_two = figure(2);
bar_handle = bar([2, 5, 3]);
hold on
scatter_handle = scatter([1, 2, 3], [3, 1, 4]);
stem_handle = stem([1, 2, 3], [2, 4, 1]);
legend_handle = legend("Bars", "Dots", "Stems");
