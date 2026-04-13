clear all
clearvars -except keep
save vars.mat alpha beta
save vars.mat -append alpha
save vars.mat -regexp ^alpha
load vars.mat alpha
load vars.mat -regexp ^alpha
who -file vars.mat
whos -file vars.mat
format short
format short g
format long
format long g
format bank
format compact
format loose
format short g compact
