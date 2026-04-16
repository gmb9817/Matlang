cells = {struct('score', 7), struct('score', 9)};
[a, b] = cells{:}.score;
[x, y] = deal(cells{:}.score);
row = [cells{:}.score];
copy = {cells{:}.score};

nested = {struct('inner', struct('score', 1)), struct('inner', struct('score', 2))};
[n1, n2] = nested{:}.inner.score;
pair = [nested{:}.inner.score];

assigned = {struct('score', 0), struct('score', 0)};
[assigned{:}.score] = deal(11, 13);
assigned_scores = [assigned{:}.score];

source_scores = {21, 23};
[assigned{:}.score] = source_scores{:};
reassigned_scores = [assigned{:}.score];

nested_assigned = {struct('inner', struct('score', 0)), struct('inner', struct('score', 0))};
[nested_assigned{:}.inner.score] = deal(31, 33);
nested_assigned_scores = [nested_assigned{:}.inner.score];

nested_source_scores = {51, 53};
[nested_assigned{:}.inner.score] = nested_source_scores{:};
nested_reassigned_scores = [nested_assigned{:}.inner.score];

synthesized = {struct(), struct()};
[synthesized{:}.inner.score] = deal(41, 43);
synthesized_scores = [synthesized{:}.inner.score];

synthesized_source_scores = {61, 63};
[synthesized{:}.inner.score] = synthesized_source_scores{:};
synthesized_reassigned_scores = [synthesized{:}.inner.score];

cell_arrays = {[struct('score', 0), struct('score', 0)], [struct('score', 0), struct('score', 0)]};
[cell_arrays{:}.score] = deal(11, 12, 13, 14);
cell_array_scores = [cell_arrays{:}.score];

cell_array_source_scores = {21, 22, 23, 24};
[cell_arrays{:}.score] = cell_array_source_scores{:};
cell_array_reassigned_scores = [cell_arrays{:}.score];

matrix_cell_arrays = {[struct('score', 0), struct('score', 0)], [struct('score', 0), struct('score', 0)]};
[matrix_cell_arrays{:}.score] = [51 52; 53 54];
matrix_cell_array_scores = [matrix_cell_arrays{:}.score];

missing_cell_structs = {struct(), struct()};
[missing_cell_structs{:}.items{:}] = deal(11, 12, 13, 14);
missing_cell_struct_row = [missing_cell_structs{:}.items{:}];

missing_cell_struct_source = {15, 16, 17, 18};
[missing_cell_structs{:}.items{:}] = missing_cell_struct_source{:};
missing_cell_struct_reassigned_row = [missing_cell_structs{:}.items{:}];

[matrix_missing_cell_structs{:}.items{:}] = [141 142; 143 144];
matrix_missing_cell_struct_row = [matrix_missing_cell_structs{:}.items{:}];

explicit_missing_cell_structs = {struct(), struct()};
[explicit_missing_cell_structs{:}.items{:}] = {41, 42, 43, 44};
explicit_missing_cell_struct_row = [explicit_missing_cell_structs{:}.items{:}];

[root_cells{:}.score] = deal(71, 73);
root_cell_scores = [root_cells{:}.score];

root_cell_source_scores = {75, 77};
[root_cells{1:2}.score] = root_cell_source_scores{:};
root_cell_reassigned_scores = [root_cells{:}.score];

[root_nested_cells{:}.inner.score] = deal(81, 83);
root_nested_scores = [root_nested_cells{:}.inner.score];

[root_cell_structs{1:2}.items{:}] = deal(91, 92, 93, 94);
root_cell_struct_row = [root_cell_structs{:}.items{:}];

[root_matrix_cell_structs{1:2}.items{:}] = [191 192; 193 194];
root_matrix_cell_struct_row = [root_matrix_cell_structs{:}.items{:}];

[root_nested_cell_structs{1:2}.inner.items{:}] = deal(101, 102, 103, 104);
root_nested_cell_struct_row = [root_nested_cell_structs{:}.inner.items{:}];

[direct_root_cell_structs{1:2}.items{:}] = [191 192 193 194];
direct_root_cell_struct_row = [direct_root_cell_structs{:}.items{:}];

[column_root_cell_structs{1:2}.items{:}] = [201; 202; 203; 204];
column_root_cell_struct_row = [column_root_cell_structs{:}.items{:}];

[deep_root.groups{:}.score] = deal(121, 123);
deep_root_scores = [deep_root.groups{:}.score];

[deep_root.groups{1:2}.items{:}] = deal(131, 132, 133, 134);
deep_root_row = [deep_root.groups{:}.items{:}];

[direct_root_cells{1:2}.score] = [171 173];
direct_root_scores = [direct_root_cells{:}.score];

[deep_direct.groups{1:2}.score] = [181 183];
deep_direct_scores = [deep_direct.groups{:}.score];

[deep_matrix.groups{1:2}.items{:}] = [301 302; 303 304];
deep_matrix_row = [deep_matrix.groups{:}.items{:}];
