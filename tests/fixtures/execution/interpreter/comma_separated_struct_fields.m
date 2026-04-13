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
