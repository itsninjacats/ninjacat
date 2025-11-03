from django.shortcuts import HttpResponse
from django.urls import path


def test(request):
    return HttpResponse("Hello, world!")


urlpatterns = [path("", test)]
