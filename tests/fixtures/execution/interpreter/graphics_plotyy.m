f = figure(47);
[ax, h1, h2] = plotyy([0, 1, 2], [1, 4, 2], [0, 1, 2], [10, 20, 30]);
ylabel("Left");
yyaxis right;
ylabel("Right");
current = gcf();
