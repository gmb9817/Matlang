data = {1, 2; 3, 4};
row = [9 8];
data(2, :) = {row, 5};
picked = data{2, 1};
result = struct();
result.value = picked;
