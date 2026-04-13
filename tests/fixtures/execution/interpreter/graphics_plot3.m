f = figure(34);
curve = plot3([0, 1, 2, 3], [0, 1, 0, 1], [0, 1, 2, 3]);
hold on
dots = scatter3([0, 1, 2], [1, 0, 1], [3, 2, 1]);
top_view = view(2);
custom_view = view(45, 20);
queried_view = view();
title("3D Plot");
current = gcf();
