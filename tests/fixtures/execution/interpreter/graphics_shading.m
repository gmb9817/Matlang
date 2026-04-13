f = figure(44);
top_axes = subplot(211);
surface_handle = surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);
default_mode = shading();
flat_mode = shading("flat");
flat_query = shading();

bottom_axes = subplot(212);
mesh_handle = mesh([0, 1, 0; 1, 2, 1; 0, 1, 0]);
bottom_default = shading();
interp_mode = shading("interp");
interp_query = shading();
top_before_target = shading(top_axes);
shading(top_axes, "flat");
top_after_target = shading(top_axes);
bottom_after_target = shading(bottom_axes);

subplot(211);
faceted_mode = shading("faceted");
top_query = shading();
current = gcf();
