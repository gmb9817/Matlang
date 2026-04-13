data = [0, 1, 2; 3, 4, 5];
f = figure(11);

top_axes = subplot(311);
heat = imagesc(data);
colormap("hot");
cb = colorbar();
heat_xlim = xlim();
heat_ylim = ylim();
initial_axis = axis();
top_scale = caxis([0, 6]);
top_scale_query = caxis();
manual_axis = axis([0.5, 3.5, 0.5, 2.5]);
auto_axis = axis("auto");

middle_axes = subplot(312);
photo = imshow([0, 0.5; 1, 0.25]);
colormap("gray");
colorbar("off");
gray_map = colormap();
middle_scale = caxis();
hidden_axis = axis();
shown_axis = axis("on");

bottom_axes = subplot(313);
indexed = image([1, 4; 8, 2]);
jet_map = colormap("jet");
direct_scale = caxis();
bottom_axis = axis("tight");
current = gcf();
