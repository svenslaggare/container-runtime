FROM ubuntu:20.04
RUN apt-get -y update && apt-get -y install iproute2 iputils-ping wget nano
RUN useradd -ms /bin/bash ubuntu
