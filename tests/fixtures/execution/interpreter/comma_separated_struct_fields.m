s = struct('name', {'alpha', 'beta', 'gamma'}, 'value', {1, 2, 3});
[n1, n2, n3] = s.name;
[v1, v2, v3] = deal(s.value);
value_row = [s.value];
name_cells = {s.name};
value_tableau = [s.value; s.value];

payload = struct('item', {struct('score', 7), struct('score', 9)});
[p1, p2] = payload.item;
item_cells = {payload.item};
score1 = p1.score;
score2 = p2.score;

assigned = struct('x', {0, 0});
[assigned.x] = deal(10, 20);
assigned_values = [assigned.x];

assigned_from_cells = struct('x', {0, 0});
source_cells = {11, 13};
[assigned_from_cells.x] = source_cells{:};
assigned_from_cells_values = [assigned_from_cells.x];

[colon_field_synth(:).score] = deal(161, 163);
colon_field_scores = [colon_field_synth.score];

[colon_field_nested.inner(:).score] = deal(171, 173);
colon_field_nested_scores = [colon_field_nested.inner.score];

[colon_brace_structs(:).items{1:2}] = deal(181, 182, 183, 184);
colon_brace_row = [colon_brace_structs.items{:}];

[colon_brace_nested.inner(:).items{1:2}] = deal(191, 192, 193, 194);
colon_brace_nested_row = [colon_brace_nested.inner.items{:}];

grid = [struct() struct(); struct() struct()];
grid.name = ["alpha" "beta"; "gamma" "delta"];
grid.value = [1 2; 3 4];
grid_names = [grid.name];
grid_values = [grid.value];

synth_struct = struct('name', {'a', 'b'});
[synth_struct.inner.score] = deal(81, 83);
synth_scores = [synth_struct.inner.score];

synth_source_scores = {85, 87};
[synth_struct.inner.score] = synth_source_scores{:};
synth_reassigned_scores = [synth_struct.inner.score];

struct_cells = struct('items', {{0, 0}, {0, 0}});
[struct_cells.items{:}] = deal(1, 2, 3, 4);
struct_cell_row = [struct_cells.items{:}];

struct_cell_source = {5, 6, 7, 8};
[struct_cells.items{:}] = struct_cell_source{:};
struct_cell_reassigned_row = [struct_cells.items{:}];

explicit_struct_cells = struct('items', {{0, 0}, {0, 0}});
[explicit_struct_cells.items{:}] = {9, 10, 11, 12};
explicit_struct_cell_row = [explicit_struct_cells.items{:}];

missing_struct_cells = struct('name', {'a', 'b'});
[missing_struct_cells.items{:}] = deal(1, 2, 3, 4);
missing_struct_cell_row = [missing_struct_cells.items{:}];

missing_struct_cell_source = {5, 6, 7, 8};
[missing_struct_cells.items{:}] = missing_struct_cell_source{:};
missing_struct_cell_reassigned_row = [missing_struct_cells.items{:}];

explicit_missing_struct_cells = struct('name', {'a', 'b'});
[explicit_missing_struct_cells.items{:}] = {1, 2, 3, 4};
explicit_missing_struct_cell_row = [explicit_missing_struct_cells.items{:}];

[synth_root_cells.items{:}] = deal(21, 22, 23, 24);
synth_root_cell_row = [synth_root_cells.items{:}];

[synth_root_col.items{:, 1}] = deal(31, 32);
synth_root_col_row = [synth_root_col.items{:}];

[indexed_synth_struct_cells(1:2).items{:}] = deal(101, 102, 103, 104);
indexed_synth_struct_row = [indexed_synth_struct_cells.items{:}];

[deep_synth.inner(1:2).items{:}] = deal(111, 112, 113, 114);
deep_synth_row = [deep_synth.inner.items{:}];

[direct_struct_cells.items{:}] = [211 212 213 214];
direct_struct_cell_row = [direct_struct_cells.items{:}];

direct_struct_matrix = struct('items', {{0, 0}, {0, 0}});
[direct_struct_matrix.items{:}] = [241 242; 243 244];
direct_struct_matrix_row = [direct_struct_matrix.items{:}];

[direct_indexed_struct(1:2).items{:}] = [221 222 223 224];
direct_indexed_struct_row = [direct_indexed_struct.items{:}];

[column_struct_cells.items{:}] = [231; 232; 233; 234];
column_struct_cell_row = [column_struct_cells.items{:}];

[deep_field_synth.groups(1:2).score] = deal(121, 123);
deep_field_scores = [deep_field_synth.groups.score];

[deep_field_synth.groups(1:2).inner.score] = deal(131, 133);
deep_field_nested_scores = [deep_field_synth.groups.inner.score];

[direct_field_matrix.groups(1:2).score] = [141 143];
direct_field_matrix_scores = [direct_field_matrix.groups.score];

[direct_field_nested_matrix.groups(1:2).inner.score] = [151 153];
direct_field_nested_matrix_scores = [direct_field_nested_matrix.groups.inner.score];
