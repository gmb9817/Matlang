groups = {{1, 2}, {3, 4}};
[a, b, c, d] = groups{:}{:};
[x, y] = groups{2}{:};
row = [groups{:}{:}];
copy = {groups{:}{:}};

matrix_cells = {{10, 20; 30, 40}, {50, 60; 70, 80}};
[u, v, w, z] = matrix_cells{:}{:, 2};
column = {matrix_cells{:}{:, 2}};

assigned_groups = {{0, 0}, {0, 0}};
[assigned_groups{:}{:}] = deal(11, 12, 13, 14);
assigned_row = [assigned_groups{:}{:}];

source_groups = {21, 22, 23, 24};
[assigned_groups{:}{:}] = source_groups{:};
reassigned_row = [assigned_groups{:}{:}];

explicit_groups = {{0, 0}, {0, 0}};
[explicit_groups{:}{:}] = {31, 32, 33, 34};
explicit_row = [explicit_groups{:}{:}];

matrix_groups = {{0, 0}, {0, 0}};
[matrix_groups{:}{:}] = [5 6; 7 8];
matrix_row = [matrix_groups{:}{:}];
