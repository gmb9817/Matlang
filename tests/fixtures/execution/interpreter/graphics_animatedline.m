f = figure(66);
h = animatedline('Color', 'r', 'Marker', 'o');
addpoints(h, [0, 1, 2], [0, 1, 0]);
[gx, gy] = getpoints(h);
xdata = get(h, 'XData');
ydata = get(h, 'YData');
drawnow;
clearpoints(h);
after_clear = get(h, 'YData');

h3 = animatedline('Color', 'b');
addpoints(h3, [0, 1], [0, 1], [1, 2]);
[gx3, gy3, gz3] = getpoints(h3);
zdata3 = get(h3, 'ZData');
type1 = get(h, 'Type');
