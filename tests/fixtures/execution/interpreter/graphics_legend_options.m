f = figure(43);
plot([1, 2, 3], [1, 4, 2]);
hold on
plot([1, 2, 3], [2, 1, 3]);
hleg = legend("First", "Second", "Location", "southwest", "Orientation", "horizontal");
current = gcf();
