f = figure(7);
top = subplot(211);
plot([0, 1, 2], [0, 1, 4]);
title("Top");
xlabel("x");
ylabel("top");

bottom = subplot(2, 1, 2);
bar([1, 2, 3], [3, 1, 2]);
legend("Bars");
current = gcf();

subplot(2, 1, 1);
top_xlim = xlim();
subplot(212);
bottom_ylim = ylim();
