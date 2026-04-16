cells = {10, 20, 30};
[a, b, c] = cells{:};
[d, e] = cells{2:3};
[x, y, z] = deal(cells{:});
row = [cells{:}];
copy = {cells{:}};
tableau = [cells{:}; cells{:}];

labels = {'north', 'east'; 'south', 'west'};
[g, h] = labels{:, 2};
[p, q, r, s] = deal(labels{:});
column = {labels{:, 2}};

updated = {0, 0, 0};
[updated{:}] = deal(100, 200, 300);
missing_root_matrix = [1 2; 3 4];
[missing_root_cells{:}] = missing_root_matrix;
missing_root_row = [missing_root_cells{:}];

matrix_cells = {0, 0; 0, 0};
[matrix_cells{1, :}] = deal(1, 2);
matrix_rhs_cells = {0, 0; 0, 0};
[matrix_rhs_cells{:}] = [1 2; 3 4];
matrix_rhs_row = [matrix_rhs_cells{:}];
nested_cells = {{0, 0}, {0, 0}};
[nested_cells{1:2}{:}] = [5 6; 7 8];
nested_rhs_row = [nested_cells{1:2}{:}];
plain_deal_cells = {0, 0};
plain_deal_cells{1:2} = deal(10, 20);
