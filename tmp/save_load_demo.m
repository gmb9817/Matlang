x = 42;
y = 'demo';
save('tmp/save_load_demo.mat', 'x', 'y');
clear('x', 'y');
load('tmp/save_load_demo.mat');
