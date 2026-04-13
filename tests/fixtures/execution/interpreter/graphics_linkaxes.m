f = figure(207);
ax1 = subplot(211);
plot([0, 1, 2], [0, 1, 0]);
ax2 = subplot(212);
plot([10, 20, 30], [1, 2, 3]);

linkaxes([ax1, ax2], 'x');
subplot(211);
xlim([5, 25]);
x_linked = get(ax2, 'XLim');

linkaxes([ax1, ax2], 'y');
subplot(212);
ylim([0, 10]);
y_linked = get(ax1, 'YLim');

linkaxes([ax1, ax2], 'off');
subplot(212);
ylim([1, 3]);
y_unlinked = get(ax1, 'YLim');
