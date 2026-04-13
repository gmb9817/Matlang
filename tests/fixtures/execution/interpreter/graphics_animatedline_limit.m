f = figure(67);
h = animatedline('MaximumNumPoints', 3);
addpoints(h, [1, 2, 3, 4], [10, 20, 30, 40]);
[x, y] = getpoints(h);
limit = get(h, 'MaximumNumPoints');
