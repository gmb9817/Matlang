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

missing_cell_structs = {struct(), struct()};
[missing_cell_structs{:}.items{:}] = deal(11, 12, 13, 14);
missing_cell_struct_row = [missing_cell_structs{:}.items{:}];

missing_cell_struct_source = {15, 16, 17, 18};
[missing_cell_structs{:}.items{:}] = missing_cell_struct_source{:};
missing_cell_struct_reassigned_row = [missing_cell_structs{:}.items{:}];

explicit_missing_cell_structs = {struct(), struct()};
[explicit_missing_cell_structs{:}.items{:}] = {41, 42, 43, 44};
explicit_missing_cell_struct_row = [explicit_missing_cell_structs{:}.items{:}];
