f = figure(43);
poly = patch([0, 1, 1, 0], [0, 0, 1, 1], 'r');
fill_handle = fill([2, 3, 3, 2], [0, 0, 1, 1], 'c');

poly_type = get(poly, 'Type');
poly_face = get(poly, 'FaceColor');
poly_edge = get(poly, 'EdgeColor');
fill_face = get(fill_handle, 'FaceColor');

set(poly, 'FaceColor', 'none');
set(poly, 'EdgeColor', [0, 0, 1]);
updated_face = get(poly, 'FaceColor');
updated_edge = get(poly, 'EdgeColor');

found_patch = findobj(f, 'Type', 'patch');
cyan_patch = findall(f, 'FaceColor', [0, 0.8, 0.8]);
patch_parent = ancestor(poly, 'axes');
